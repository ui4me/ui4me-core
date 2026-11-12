//! Read-only document text selection (Core 0.3 S4).
//!
//! A selection point is anchored to the DOM, not to a layout epoch: the inline root element of a
//! paragraph, the ordinal of that paragraph among the root's paragraphs (anonymous blocks may give
//! one element several), and a UTF-8 byte offset into the paragraph's processed text. Mutations
//! elsewhere (a growing log) leave a selection intact; text replaced inside the selected
//! paragraph clamps its offsets; a removed root clears it.
//!
//! Paragraphs are ordered as layout lists them (document order for in-flow content). The
//! contents of form controls and generated (`::before`/`::after`) text are never part of the
//! document selection, so a password value cannot be selected or copied through it.
//! The highlight is a translucent overlay blended over the rasterized frame, clipped to the
//! scrollports of the text's scroll containers.
use super::*;
use crate::layout::fragment::{FragmentId, FragmentKind, FragmentTree};
use std::collections::HashMap;

/// One DOM-anchored selection end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelPoint {
    root: NodeId,
    nth: u32,
    byte: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Unit {
    Char,
    /// The anchor unit of a double/triple-click drag, in paragraph-index space.
    Range(usize, usize, usize, usize),
}

#[derive(Debug, Default)]
pub(super) struct DocSelection {
    anchor: Option<SelPoint>,
    focus: Option<SelPoint>,
    dragging: bool,
    unit: Option<Unit>,
    last_click: Option<(Instant, f64, f64, u8)>,
}

/// Paragraph-index resolved point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Resolved {
    para: usize,
    byte: usize,
}

struct Para {
    root: NodeId,
    nth: u32,
    frags: Vec<FragmentId>,
}

/// The selectable paragraphs of the committed layout.
struct TextIndex {
    paras: HashMap<usize, Para>,
    by_root: HashMap<(NodeId, u32), usize>,
    order: Vec<usize>,
}

type Area = (f64, f64, f64, f64);
type Ordered = (
    Arc<crate::pipeline::publish::CommittedSet>,
    TextIndex,
    Resolved,
    Resolved,
);

const HIGHLIGHT: (u32, u32, u32) = (51, 144, 255);
const HIGHLIGHT_ALPHA: u32 = 90; // of 255
const MULTI_CLICK: Duration = Duration::from_millis(500);

fn in_control(d: &Model, n: NodeId) -> bool {
    d.arena().inclusive_ancestors(n).into_iter().any(|a| {
        d.local_name(a)
            .is_ok_and(|t| matches!(t, "input" | "textarea" | "select" | "button"))
    })
}

fn is_control(c: char) -> bool {
    matches!(c, '\u{200B}' | '\u{200E}' | '\u{200F}' | '\u{FEFF}'
        | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn strip_controls(text: &str) -> String {
    text.chars().filter(|c| !is_control(*c)).collect()
}

fn floor_boundary(text: &str, byte: usize) -> usize {
    let mut b = byte.min(text.len());
    while !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

impl TextIndex {
    fn build(tree: &FragmentTree, d: &Model) -> Self {
        // A paragraph's identity is the element of the block that contains its lines (the inline
        // root), found from its first text fragment.
        let mut frags: HashMap<usize, (NodeId, Vec<FragmentId>)> = HashMap::new();
        for (i, f) in tree.fragments.iter().enumerate() {
            if let FragmentKind::Text(tf) = &f.kind
                && tf.pseudo.is_none()
            {
                let id = FragmentId(i as u32);
                match frags.get_mut(&tf.paragraph_index) {
                    Some(p) => p.1.push(id),
                    None => {
                        if let Some(root) = Document::container_of(tree, id) {
                            frags.insert(tf.paragraph_index, (root, vec![id]));
                        }
                    }
                }
            }
        }
        let mut paras = HashMap::new();
        let mut by_root = HashMap::new();
        let mut order: Vec<usize> = frags.keys().copied().collect();
        order.sort_unstable();
        let mut counts: HashMap<NodeId, u32> = HashMap::new();
        order.retain(|&i| {
            let (root, list) = frags.remove(&i).expect("indexed paragraph");
            let nth = counts.entry(root).or_insert(0);
            let this = *nth;
            *nth += 1;
            if !d.arena().contains(root) || in_control(d, root) {
                return false;
            }
            by_root.insert((root, this), i);
            paras.insert(
                i,
                Para {
                    root,
                    nth: this,
                    frags: list,
                },
            );
            true
        });
        Self {
            paras,
            by_root,
            order,
        }
    }
    fn resolve(&self, tree: &FragmentTree, p: SelPoint) -> Option<Resolved> {
        let para = *self.by_root.get(&(p.root, p.nth))?;
        let text = tree.paragraphs.get(para)?.text();
        Some(Resolved {
            para,
            byte: floor_boundary(text, p.byte),
        })
    }
    fn point(&self, r: Resolved) -> Option<SelPoint> {
        let p = self.paras.get(&r.para)?;
        Some(SelPoint {
            root: p.root,
            nth: p.nth,
            byte: r.byte,
        })
    }
}

impl Document {
    fn committed_tree(&mut self) -> Result<Option<Arc<crate::pipeline::publish::CommittedSet>>> {
        self.pipeline.flush()?;
        Ok(self.pipeline.committed.clone())
    }
    /// The viewport offset of content inside `container`'s scroll chain (its own scroll included).
    fn content_scroll(&self, container: Option<NodeId>) -> (f64, f64) {
        let chain = match container {
            Some(n) => self.pipeline.scroll_chain(n),
            None => self
                .pipeline
                .scroll_entry(ScrollTarget::Viewport)
                .into_iter()
                .collect(),
        };
        chain
            .iter()
            .fold((0.0, 0.0), |a, e| (a.0 + e.offset.0, a.1 + e.offset.1))
    }
    fn container_of(tree: &FragmentTree, f: FragmentId) -> Option<NodeId> {
        let mut at = tree.get(f).parent;
        while let Some(id) = at {
            if let FragmentKind::Box(b) = &tree.get(id).kind
                && let Some(n) = b.owner_node()
            {
                return Some(n);
            }
            at = tree.get(id).parent;
        }
        None
    }
    /// Viewport geometry of one text fragment: its line's vertical extent, its horizontal origin
    /// and the paragraph-space x of its first cluster.
    fn frag_geometry(
        &self,
        tree: &FragmentTree,
        f: FragmentId,
        scroll: &mut HashMap<Option<NodeId>, (f64, f64)>,
    ) -> Option<(
        f64,
        f64,
        f64,
        f64,
        f64,
        Vec<crate::text::paragraph::frozen::ClusterExtent>,
    )> {
        let FragmentKind::Text(tf) = &tree.get(f).kind else {
            return None;
        };
        let container = Self::container_of(tree, f);
        let (sx, sy) = *scroll
            .entry(container)
            .or_insert_with(|| self.content_scroll(container));
        let r = tree.document_rect(f).rect;
        let line = tree
            .get(f)
            .parent
            .map(|l| tree.document_rect(l).rect)
            .unwrap_or(r);
        let p = tree.paragraphs.get(tf.paragraph_index)?;
        let clusters: Vec<_> = p
            .cluster_extents(tf.line_index)
            .into_iter()
            .filter(|c| {
                c.start >= tf.text_range.start && c.end <= tf.text_range.end && c.end > c.start
            })
            .collect();
        let base = clusters.iter().map(|c| c.x0).fold(f32::INFINITY, f32::min);
        let base = if base.is_finite() {
            f64::from(base)
        } else {
            0.0
        };
        Some((
            r.origin.x.to_f64() - sx,
            line.origin.y.to_f64() - sy,
            r.size.width.to_f64(),
            line.size.height.to_f64(),
            base,
            clusters,
        ))
    }
    /// The selectable text position nearest to a viewport point.
    /// The element whose text a pointer at (x, y) selects: the nearest inclusive ancestor of
    /// the hit element that contains selectable text. `None` (nothing hit, or no text above it)
    /// lets the whole document compete, as when dragging outside every box.
    fn text_scope(&mut self, index: &TextIndex, x: f64, y: f64) -> Result<Option<NodeId>> {
        let id = self.id();
        let Some(hit) = self.hit_test(id, x, y)? else {
            return Ok(None);
        };
        let d = self.pipeline.document();
        let roots: Vec<NodeId> = index.paras.values().map(|p| p.root).collect();
        Ok(d.arena()
            .inclusive_ancestors(hit)
            .into_iter()
            .find(|&a| roots.iter().any(|&r| d.arena().is_inclusive_ancestor(a, r))))
    }
    fn position_at(
        &self,
        tree: &FragmentTree,
        index: &TextIndex,
        scope: Option<NodeId>,
        x: f64,
        y: f64,
    ) -> Option<Resolved> {
        let mut scroll = HashMap::new();
        let mut best: Option<((f64, f64), Resolved)> = None;
        let d = self.pipeline.document();
        for &para in index.order.iter().filter(|p| {
            scope.is_none_or(|s| d.arena().is_inclusive_ancestor(s, index.paras[p].root))
        }) {
            for &f in &index.paras[&para].frags {
                let Some((fx, fy, fw, fh, base, clusters)) =
                    self.frag_geometry(tree, f, &mut scroll)
                else {
                    continue;
                };
                let dy = if y < fy {
                    fy - y
                } else if y > fy + fh {
                    y - fy - fh
                } else {
                    0.0
                };
                let dx = if x < fx {
                    fx - x
                } else if x > fx + fw {
                    x - fx - fw
                } else {
                    0.0
                };
                if best.as_ref().is_some_and(|(d, _)| (dy, dx) >= *d) {
                    continue;
                }
                let FragmentKind::Text(tf) = &tree.get(f).kind else {
                    continue;
                };
                let mut byte = tf.text_range.start;
                for c in &clusters {
                    let x0 = fx + f64::from(c.x0) - base;
                    let x1 = fx + f64::from(c.x1) - base;
                    let (lead, trail) = if c.rtl {
                        (c.end, c.start)
                    } else {
                        (c.start, c.end)
                    };
                    if x >= (x0 + x1) / 2.0 {
                        byte = trail;
                    } else {
                        byte = lead;
                        if !c.rtl {
                            break;
                        }
                    }
                }
                best = Some(((dy, dx), Resolved { para, byte }));
            }
        }
        best.map(|(_, r)| r)
    }
    fn ordered(&mut self) -> Result<Option<Ordered>> {
        let (Some(a), Some(f)) = (self.selection.anchor, self.selection.focus) else {
            return Ok(None);
        };
        let Some(c) = self.committed_tree()? else {
            return Ok(None);
        };
        let tree = &c.layout.fragments;
        let index = TextIndex::build(tree, self.pipeline.document());
        let (Some(a), Some(f)) = (index.resolve(tree, a), index.resolve(tree, f)) else {
            self.selection.anchor = None;
            self.selection.focus = None;
            self.repaint = true;
            return Ok(None);
        };
        let (start, end) = if a <= f { (a, f) } else { (f, a) };
        if start == end {
            return Ok(None);
        }
        Ok(Some((c, index, start, end)))
    }
    /// The selected text: each paragraph's rendered text, paragraphs joined by a line feed.
    pub fn selection_text(&mut self) -> Result<String> {
        let Some((c, index, start, end)) = self.ordered()? else {
            return Ok(String::new());
        };
        let tree = &c.layout.fragments;
        let mut out = String::new();
        let mut first = true;
        for &para in index
            .order
            .iter()
            .filter(|&&p| p >= start.para && p <= end.para)
        {
            let text = tree.paragraphs[para].text();
            let s = if para == start.para { start.byte } else { 0 };
            let e = if para == end.para {
                end.byte
            } else {
                text.len()
            };
            if !first {
                out.push('\n');
            }
            first = false;
            out.push_str(&strip_controls(text.get(s..e).unwrap_or("")));
            if out.len() > 16 * 1024 * 1024 {
                return Err(limit_error("Selected text exceeds 16 MiB"));
            }
        }
        Ok(out)
    }
    pub fn clear_selection(&mut self) {
        let before = (self.selection.anchor, self.selection.focus);
        self.clear_selection_quiet();
        self.note_selection_change(before);
    }
    fn clear_selection_quiet(&mut self) {
        if self.selection.anchor.is_some() {
            self.repaint = true;
        }
        self.selection.anchor = None;
        self.selection.focus = None;
        self.selection.dragging = false;
        self.selection.unit = None;
    }
    /// Editing hosts (0.8 F0801): the selection inside `host`'s single text paragraph as byte
    /// offsets `(anchor, focus)`, when both ends are there.
    pub(super) fn host_selection(&self, host: NodeId) -> Option<(usize, usize)> {
        let (a, f) = (self.selection.anchor?, self.selection.focus?);
        if self.is_rich_host(host) {
            // Rich hosts: the points are in the host's leaf blocks; flat offsets join them.
            let r = self.rich_load(host);
            let ids = r.block_ids();
            // Loose text directly in the host (`<div contenteditable>text</div>`) is one
            // synthetic block with no element of its own: its points are rooted at the host.
            let loose = ids.len() == 1 && ids[0].is_none();
            let flat = |p: SelPoint| {
                let i = if loose && p.root == host {
                    0
                } else {
                    ids.iter().position(|x| *x == Some(p.root))?
                };
                (p.nth == 0).then(|| r.flat_of(i, p.byte))
            };
            return Some((flat(a)?, flat(f)?));
        }
        (a.root == host && f.root == host && a.nth == 0 && f.nth == 0).then_some((a.byte, f.byte))
    }
    /// Place the selection inside `host` (flat byte offsets for a rich host).
    pub(super) fn set_host_selection(&mut self, host: NodeId, anchor: usize, focus: usize) {
        if self.is_rich_host(host) {
            let r = self.rich_load(host);
            self.set_rich_selection(host, &r, anchor, focus);
            return;
        }
        let point = |byte| SelPoint {
            root: host,
            nth: 0,
            byte,
        };
        self.place_selection(point(anchor), point(focus));
    }
    /// Place the selection of a rich host from flat offsets, using its current tree.
    pub(super) fn set_rich_selection(
        &mut self,
        host: NodeId,
        r: &super::rich_edit::Rich,
        anchor: usize,
        focus: usize,
    ) {
        let ids = r.block_ids();
        let loose = ids.len() == 1 && ids[0].is_none();
        let point = |flat: usize| {
            let (i, byte) = r.locate(flat);
            let root = ids.get(i).copied().flatten();
            // The single synthetic block of loose host text is addressed through the host.
            root.or(loose.then_some(host))
                .map(|root| SelPoint { root, nth: 0, byte })
        };
        if let (Some(a), Some(f)) = (point(anchor), point(focus)) {
            self.place_selection(a, f);
        }
    }
    fn place_selection(&mut self, anchor: SelPoint, focus: SelPoint) {
        let before = (self.selection.anchor, self.selection.focus);
        self.selection.anchor = Some(anchor);
        self.selection.focus = Some(focus);
        self.selection.dragging = false;
        self.selection.unit = None;
        self.repaint = true;
        self.note_selection_change(before);
    }
    /// The lines of `host`'s text: byte range, viewport top and height, and the caret stops
    /// `(byte, viewport x)` in logical order (flat offsets for a rich host).
    #[allow(clippy::type_complexity)]
    pub(super) fn host_lines(
        &mut self,
        host: NodeId,
    ) -> Result<Vec<(std::ops::Range<usize>, f64, f64, Vec<(usize, f64)>)>> {
        if !self.is_rich_host(host) {
            return self.root_lines(host, 0);
        }
        let r = self.rich_load(host);
        let mut out = Vec::new();
        for (i, id) in r.block_ids().into_iter().enumerate() {
            let Some(id) = id else { continue };
            let base = r.flat_of(i, 0);
            let lines = self.root_lines(id, base)?;
            if lines.is_empty() {
                // An empty paragraph has no text line: its box stands in for one.
                if let Some((x, y, _, h)) = self.viewport_rect(id) {
                    out.push((base..base, y, h.clamp(10.0, 40.0), vec![(base, x)]));
                }
            }
            out.extend(lines);
        }
        Ok(out)
    }
    /// The lines of the paragraph rooted at `root`, byte offsets shifted by `base`.
    #[allow(clippy::type_complexity)]
    fn root_lines(
        &mut self,
        root: NodeId,
        base: usize,
    ) -> Result<Vec<(std::ops::Range<usize>, f64, f64, Vec<(usize, f64)>)>> {
        let Some(c) = self.committed_tree()? else {
            return Ok(Vec::new());
        };
        let tree = &c.layout.fragments;
        let index = TextIndex::build(tree, self.pipeline.document());
        let Some(&para) = index.by_root.get(&(root, 0)) else {
            return Ok(Vec::new());
        };
        let p = &tree.paragraphs[para];
        let mut scroll = HashMap::new();
        let mut out: Vec<(std::ops::Range<usize>, f64, f64, Vec<(usize, f64)>)> = Vec::new();
        for &f in &index.paras[&para].frags {
            let FragmentKind::Text(tf) = &tree.get(f).kind else {
                continue;
            };
            let Some((fx, fy, _, fh, fbase, _)) = self.frag_geometry(tree, f, &mut scroll) else {
                continue;
            };
            if out.iter().any(|l| l.1 == fy) {
                continue;
            }
            let line = &p.lines()[tf.line_index];
            let stops = p
                .caret_stops(tf.line_index)
                .stops
                .iter()
                .map(|s| (s.byte + base, fx + f64::from(s.x) - fbase))
                .collect();
            out.push((
                line.text_range.start + base..line.text_range.end + base,
                fy,
                fh,
                stops,
            ));
        }
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        Ok(out)
    }
    /// Select all document text inside `element` (or the whole document for `None`).
    pub fn select_text(&mut self, element: Option<&str>) -> Result<()> {
        let scope = element.map(|e| self.node(e)).transpose()?;
        let before = (self.selection.anchor, self.selection.focus);
        self.clear_selection_quiet();
        let result = self.select_text_inner(scope);
        self.note_selection_change(before);
        result
    }
    fn select_text_inner(&mut self, scope: Option<NodeId>) -> Result<()> {
        let Some(c) = self.committed_tree()? else {
            return Ok(());
        };
        let tree = &c.layout.fragments;
        let d = self.pipeline.document();
        let index = TextIndex::build(tree, d);
        let mut first: Option<Resolved> = None;
        let mut last: Option<Resolved> = None;
        for &para in &index.order {
            for &f in &index.paras[&para].frags {
                let FragmentKind::Text(tf) = &tree.get(f).kind else {
                    continue;
                };
                if scope.is_some_and(|s| !d.arena().is_inclusive_ancestor(s, tf.owner)) {
                    continue;
                }
                let a = Resolved {
                    para,
                    byte: tf.text_range.start,
                };
                let b = Resolved {
                    para,
                    byte: tf.text_range.end,
                };
                first = Some(first.map_or(a, |x| x.min(a)));
                last = Some(last.map_or(b, |x| x.max(b)));
            }
        }
        if let (Some(a), Some(b)) = (first, last) {
            self.selection.anchor = index.point(a);
            self.selection.focus = index.point(b);
            self.repaint = true;
        }
        Ok(())
    }
    /// Copy the selection to the clipboard; returns the copied text (empty: nothing copied).
    pub fn copy_selection(&mut self) -> Result<String> {
        let text = self.selection_text()?;
        if !text.is_empty() {
            self.set_clipboard_text(&text)?;
        }
        Ok(text)
    }
    pub fn clipboard_text(&mut self) -> Result<Option<String>> {
        if self.native_clipboard.is_enabled() {
            return Err(arg(
                "Native clipboard reads use the asynchronous host service",
            ));
        }
        self.clipboard.read_text().map_err(|e| Error {
            code: 6,
            message: format!("Clipboard read failed: {e}"),
        })
    }
    pub fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        if text.len() > 16 * 1024 * 1024 || text.contains('\0') {
            return Err(arg("Clipboard text must be at most 16 MiB without NUL"));
        }
        if self.defer_native_write(self.id(), None, &ClipboardData::text(text), false) {
            return Ok(());
        }
        self.clipboard.write_text(text).map_err(|e| Error {
            code: 6,
            message: format!("Clipboard write failed: {e}"),
        })
    }
    fn focus_in_control(&self) -> bool {
        let d = self.pipeline.document();
        self.interaction
            .focused_element(self.id())
            .is_some_and(|n| in_control(d, n))
    }
    /// Pointer/keyboard handling after the engine processed the input (its events are `events`).
    /// Queue `selectionchange` when the selection differs from `before`.
    fn note_selection_change(&mut self, before: (Option<SelPoint>, Option<SelPoint>)) {
        if before != (self.selection.anchor, self.selection.focus) && self.events.len() < 1024 {
            self.events
                .push(serde_json::json!({"type": "selectionchange"}));
        }
    }
    /// While dragging a selection past the edge of the scroll container that holds the anchor
    /// (innermost first, then the viewport), scroll it toward the pointer.
    fn drag_autoscroll(&mut self, y: f64) -> Result<()> {
        let Some(anchor) = self.selection.anchor else {
            return Ok(());
        };
        let chain: Vec<ScrollTarget> = self
            .pipeline
            .scroll_chain(anchor.root)
            .into_iter()
            .filter(|e| e.user_y && e.range.max.1 > e.range.min.1)
            .map(|e| e.target)
            .collect();
        let id = self.id();
        for target in chain {
            let Some((_, top, _, height)) = self.scrollport_rect(target) else {
                continue;
            };
            let dy = if y < top {
                (y - top).max(-40.0)
            } else if y > top + height {
                (y - top - height).min(40.0)
            } else {
                0.0
            };
            if dy == 0.0 {
                return Ok(());
            }
            let Some(e) = self.pipeline.scroll_entry(target) else {
                continue;
            };
            let node = match target {
                ScrollTarget::Viewport => None,
                ScrollTarget::Node(n) => Some(n),
            };
            let applied =
                crate::interaction::scroll::scroll_to(self, id, node, e.offset.0, e.offset.1 + dy)?;
            if applied.changed {
                self.repaint = true;
                return Ok(());
            }
        }
        Ok(())
    }
    pub(super) fn selection_input(&mut self, input: &Input, events: &[Value]) -> Result<()> {
        let before = (self.selection.anchor, self.selection.focus);
        let result = self.selection_input_inner(input, events);
        self.note_selection_change(before);
        result
    }
    fn selection_input_inner(&mut self, input: &Input, events: &[Value]) -> Result<()> {
        let prevented = |ty: &str| {
            events
                .iter()
                .any(|e| e["type"] == ty && e["default_prevented"] == true)
        };
        match *input {
            Input::PointerButton {
                pressed: true,
                button: 0,
                x,
                y,
            } => {
                if prevented("mousedown") {
                    return Ok(());
                }
                let id = self.id();
                let hit = self.hit_test(id, x, y)?;
                let d = self.pipeline.document();
                if let Some(n) = hit.filter(|&n| in_control(d, n)) {
                    // A press into a text-entry control moves the selection into it; buttons,
                    // checkboxes and similar controls keep the document selection (so a "Copy"
                    // button can copy it).
                    let entry = d.arena().inclusive_ancestors(n).into_iter().any(|a| {
                        crate::interaction::editing::text_kind(d, a).is_some()
                            || d.local_name(a).is_ok_and(|t| t == "select")
                    });
                    if entry {
                        self.clear_selection_quiet();
                    }
                    return Ok(());
                }
                let Some(c) = self.committed_tree()? else {
                    return Ok(());
                };
                let tree = &c.layout.fragments;
                let index = TextIndex::build(tree, self.pipeline.document());
                let scope = self.text_scope(&index, x, y)?;
                let Some(at) = self.position_at(tree, &index, scope, x, y) else {
                    self.clear_selection_quiet();
                    return Ok(());
                };
                let now = Instant::now();
                let count = match self.selection.last_click {
                    Some((t, px, py, n))
                        if now.duration_since(t) < MULTI_CLICK
                            && (px - x).abs() < 4.0
                            && (py - y).abs() < 4.0 =>
                    {
                        n % 3 + 1
                    }
                    _ => 1,
                };
                self.selection.last_click = Some((now, x, y, count));
                let shift = self.modifiers.shift;
                let text = tree.paragraphs[at.para].text();
                let (a, b) = match count {
                    2 => word_at(text, at.byte),
                    3 => (0, text.len()),
                    _ => (at.byte, at.byte),
                };
                let (a, b) = (
                    Resolved {
                        para: at.para,
                        byte: a,
                    },
                    Resolved {
                        para: at.para,
                        byte: b,
                    },
                );
                if shift && count == 1 && self.selection.anchor.is_some() {
                    self.selection.focus = index.point(b);
                } else {
                    self.selection.anchor = index.point(a);
                    self.selection.focus = index.point(b);
                    self.selection.unit =
                        (count > 1).then_some(Unit::Range(a.para, a.byte, b.para, b.byte));
                }
                if count == 1 {
                    self.selection.unit = Some(Unit::Char);
                }
                self.selection.dragging = true;
                self.repaint = true;
            }
            Input::PointerMoved { x, y }
                if self.selection.dragging && self.pointer_buttons & 1 != 0 =>
            {
                self.drag_autoscroll(y)?;
                let Some(c) = self.committed_tree()? else {
                    return Ok(());
                };
                let tree = &c.layout.fragments;
                let index = TextIndex::build(tree, self.pipeline.document());
                let scope = self.text_scope(&index, x, y)?;
                if let Some(at) = self.position_at(tree, &index, scope, x, y) {
                    match self.selection.unit {
                        Some(Unit::Range(ap, ab, bp, bb)) => {
                            let (us, ue) = (
                                Resolved { para: ap, byte: ab },
                                Resolved { para: bp, byte: bb },
                            );
                            let text = tree.paragraphs[at.para].text();
                            let (ws, we) = match () {
                                _ if ap == bp
                                    && ab == 0
                                    && bb == tree.paragraphs[ap].text().len() =>
                                {
                                    (0, text.len())
                                }
                                _ => word_at(text, at.byte),
                            };
                            let (ws, we) = (
                                Resolved {
                                    para: at.para,
                                    byte: ws,
                                },
                                Resolved {
                                    para: at.para,
                                    byte: we,
                                },
                            );
                            if ws < us {
                                self.selection.anchor = index.point(ue);
                                self.selection.focus = index.point(ws);
                            } else {
                                self.selection.anchor = index.point(us);
                                self.selection.focus = index.point(we.max(ue));
                            }
                        }
                        _ => self.selection.focus = index.point(at),
                    }
                    self.repaint = true;
                }
            }
            Input::PointerButton {
                pressed: false,
                button: 0,
                ..
            } => self.selection.dragging = false,
            Input::Focus(false) | Input::PointerLeft if self.pointer_buttons & 1 == 0 => {
                self.selection.dragging = false
            }
            Input::Key {
                ref key,
                pressed: true,
                ctrl,
                alt: false,
                ..
            } if ctrl && !self.focus_in_control() => {
                if prevented("keydown") {
                    return Ok(());
                }
                match key.as_str() {
                    "c" | "C" | "Insert" => {
                        if prevented("copy") {
                            return Ok(());
                        }
                        // A clipboard failure must not fail the input event; it is reported as a
                        // document error like other default-action failures.
                        if let Err(e) = self.copy_selection() {
                            self.record_error(
                                self.id(),
                                Status::new(Code::InvalidArgument, Stage::Host)
                                    .with_message(e.message),
                            );
                        }
                    }
                    "a" | "A" => {
                        self.clear_selection_quiet();
                        self.select_text_inner(None)?
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// Highlight rectangles (viewport CSS px) clipped to their scrollports.
    pub(super) fn selection_rects(&mut self) -> Result<Vec<(f64, f64, f64, f64)>> {
        let Some((c, index, start, end)) = self.ordered()? else {
            return Ok(Vec::new());
        };
        let tree = &c.layout.fragments;
        let mut scroll = HashMap::new();
        let mut clips: HashMap<Option<NodeId>, Option<Area>> = HashMap::new();
        let mut out = Vec::new();
        for &para in index
            .order
            .iter()
            .filter(|&&p| p >= start.para && p <= end.para)
        {
            let p = &tree.paragraphs[para];
            let s = if para == start.para { start.byte } else { 0 };
            let e = if para == end.para {
                end.byte
            } else {
                p.text().len()
            };
            for &f in &index.paras[&para].frags {
                let FragmentKind::Text(tf) = &tree.get(f).kind else {
                    continue;
                };
                let (a, b) = (s.max(tf.text_range.start), e.min(tf.text_range.end));
                if a >= b {
                    continue;
                }
                let Some((fx, fy, _, fh, base, _)) = self.frag_geometry(tree, f, &mut scroll)
                else {
                    continue;
                };
                let container = Self::container_of(tree, f);
                let clip = *clips
                    .entry(container)
                    .or_insert_with(|| self.clip_for(container));
                for (x0, x1) in p.selection_spans(tf.line_index, a..b) {
                    let r = (fx + f64::from(x0) - base, fy, f64::from(x1 - x0), fh);
                    if let Some(r) = intersect(r, clip) {
                        out.push(r);
                    }
                }
            }
        }
        Ok(out)
    }
    /// The rendered text of `scope` for text geometry (0.6 F0603): its paragraphs (inline roots
    /// inside it, or the one paragraph containing it) in layout order, joined with `\n`, controls
    /// removed: the text a selection of it copies. Returns the paragraphs and the text.
    fn scoped_paragraphs(
        tree: &FragmentTree,
        index: &TextIndex,
        d: &Model,
        scope: NodeId,
    ) -> Vec<usize> {
        let inside: Vec<usize> = index
            .order
            .iter()
            .copied()
            .filter(|p| d.arena().is_inclusive_ancestor(scope, index.paras[p].root))
            .collect();
        if !inside.is_empty() {
            return inside;
        }
        // An inline element: the paragraph whose root contains it.
        let _ = tree;
        index
            .order
            .iter()
            .copied()
            .rfind(|p| d.arena().is_inclusive_ancestor(index.paras[p].root, scope))
            .into_iter()
            .collect()
    }
    /// Map a UTF-16 offset in the control-stripped text of `text` to a byte offset of `text`.
    fn byte_of_visible(text: &str, mut u: u32) -> usize {
        for (b, c) in text.char_indices() {
            if is_control(c) {
                continue;
            }
            if u == 0 {
                return b;
            }
            u = u.saturating_sub(c.len_utf16() as u32);
        }
        text.len()
    }
    fn visible_len(text: &str) -> u32 {
        text.chars()
            .filter(|c| !is_control(*c))
            .map(|c| c.len_utf16() as u32)
            .sum()
    }
    /// Rendered text, rectangles of its UTF-16 range `start..end` (viewport CSS px, with whether
    /// each is inside its scrollports) for `scope` (0.6 F0603). At most `limit` rectangles.
    #[allow(clippy::type_complexity)]
    pub(super) fn text_range_rects(
        &mut self,
        scope: NodeId,
        range: Option<(u32, u32)>,
        limit: usize,
    ) -> Result<(String, Vec<(f64, f64, f64, f64, bool)>)> {
        let Some(c) = self.committed_tree()? else {
            return Ok((String::new(), Vec::new()));
        };
        let tree = &c.layout.fragments;
        let index = TextIndex::build(tree, self.pipeline.document());
        let paras = Self::scoped_paragraphs(tree, &index, self.pipeline.document(), scope);
        let mut text = String::new();
        let mut spans = Vec::new(); // (para, utf16 start of the para in `text`)
        for (i, &p) in paras.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            spans.push((p, Self::visible_len(&text)));
            text.push_str(&strip_controls(tree.paragraphs[p].text()));
        }
        let total = Self::visible_len(&text);
        let (start, end) = range.unwrap_or((0, total));
        if start > end || end > total {
            return Err(arg(format!("range {start}..{end} is outside 0..{total}")));
        }
        let mut scroll = HashMap::new();
        let mut clips: HashMap<Option<NodeId>, Option<Area>> = HashMap::new();
        let mut out = Vec::new();
        for &(para, at) in &spans {
            let p = &tree.paragraphs[para];
            let plen = Self::visible_len(p.text());
            let (a, b) = (start.max(at), end.min(at + plen));
            if a >= b {
                continue;
            }
            let (a, b) = (
                Self::byte_of_visible(p.text(), a - at),
                Self::byte_of_visible(p.text(), b - at),
            );
            for &f in &index.paras[&para].frags {
                let FragmentKind::Text(tf) = &tree.get(f).kind else {
                    continue;
                };
                let (s, e) = (a.max(tf.text_range.start), b.min(tf.text_range.end));
                if s >= e {
                    continue;
                }
                let Some((fx, fy, _, fh, base, _)) = self.frag_geometry(tree, f, &mut scroll)
                else {
                    continue;
                };
                let container = Self::container_of(tree, f);
                let clip = *clips
                    .entry(container)
                    .or_insert_with(|| self.clip_for(container));
                for (x0, x1) in p.selection_spans(tf.line_index, s..e) {
                    let r = (fx + f64::from(x0) - base, fy, f64::from(x1 - x0), fh);
                    out.push((r.0, r.1, r.2, r.3, intersect(r, clip).is_some()));
                    if out.len() >= limit {
                        return Ok((text, out));
                    }
                }
            }
        }
        Ok((text, out))
    }
    /// The text block and UTF-16 offset in its rendered text nearest to a viewport point, as a
    /// click would place a selection (0.6 F0603): (inline root, offset in the nearest identified
    /// inclusive ancestor's rendered text, that ancestor).
    pub(super) fn text_position_at(
        &mut self,
        x: f64,
        y: f64,
    ) -> Result<Option<(NodeId, u32, Option<NodeId>)>> {
        let Some(c) = self.committed_tree()? else {
            return Ok(None);
        };
        let tree = &c.layout.fragments;
        let index = TextIndex::build(tree, self.pipeline.document());
        let scope = self.text_scope(&index, x, y)?;
        let Some(r) = self.position_at(tree, &index, scope, x, y) else {
            return Ok(None);
        };
        let d = self.pipeline.document();
        let root = index.paras[&r.para].root;
        let owner = d.arena().inclusive_ancestors(root).into_iter().find(|&a| {
            d.get_attribute(a, "id")
                .ok()
                .flatten()
                .is_some_and(|i| !i.is_empty())
        });
        let in_para = {
            let t = tree.paragraphs[r.para].text();
            Self::visible_len(&t[..floor_boundary(t, r.byte)])
        };
        let offset = match owner {
            Some(o) => {
                let paras = Self::scoped_paragraphs(tree, &index, d, o);
                let mut at = 0u32;
                for (i, &p) in paras.iter().enumerate() {
                    if p == r.para {
                        break;
                    }
                    at += Self::visible_len(tree.paragraphs[p].text())
                        + u32::from(i + 1 < paras.len());
                }
                at + in_para
            }
            None => in_para,
        };
        Ok(Some((root, offset, owner)))
    }
    fn clip_for(&self, container: Option<NodeId>) -> Option<(f64, f64, f64, f64)> {
        let mut clip = (
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        );
        if let Some(n) = container {
            for e in self.pipeline.scroll_chain(n) {
                if e.target == ScrollTarget::Viewport {
                    continue;
                }
                let port = self.scrollport_rect(e.target)?;
                clip = intersect_ltrb(clip, port)?;
            }
        }
        Some((clip.0, clip.1, clip.2 - clip.0, clip.3 - clip.1))
    }
    /// Blend the selection highlight over a rendered frame (device pixels, `0x00RRGGBB`).
    pub(super) fn paint_selection(
        &mut self,
        pixels: &mut [u32],
        width: u32,
        height: u32,
        scale: f64,
    ) -> Result<()> {
        if self.selection.anchor.is_none() {
            return Ok(());
        }
        for (x, y, w, h) in self.selection_rects()? {
            let x0 = ((x * scale).round().max(0.0) as u32).min(width);
            let y0 = ((y * scale).round().max(0.0) as u32).min(height);
            let x1 = (((x + w) * scale).round().max(0.0) as u32).min(width);
            let y1 = (((y + h) * scale).round().max(0.0) as u32).min(height);
            for row in y0..y1 {
                let line = &mut pixels[(row * width) as usize..((row + 1) * width) as usize];
                for px in &mut line[x0 as usize..x1 as usize] {
                    let ch = |shift: u32, c: u32| {
                        let v = (*px >> shift) & 0xff;
                        ((v * (255 - HIGHLIGHT_ALPHA) + c * HIGHLIGHT_ALPHA + 127) / 255) << shift
                    };
                    *px = ch(16, HIGHLIGHT.0) | ch(8, HIGHLIGHT.1) | ch(0, HIGHLIGHT.2);
                }
            }
        }
        Ok(())
    }
}

fn intersect_ltrb(
    a: (f64, f64, f64, f64),
    (x, y, w, h): (f64, f64, f64, f64),
) -> Option<(f64, f64, f64, f64)> {
    let r = (a.0.max(x), a.1.max(y), a.2.min(x + w), a.3.min(y + h));
    (r.0 < r.2 && r.1 < r.3).then_some(r)
}

fn intersect(
    r: (f64, f64, f64, f64),
    clip: Option<(f64, f64, f64, f64)>,
) -> Option<(f64, f64, f64, f64)> {
    let clip = clip?;
    let i = intersect_ltrb((r.0, r.1, r.0 + r.2, r.1 + r.3), clip)?;
    Some((i.0, i.1, i.2 - i.0, i.3 - i.1))
}

/// The word (a run of alphanumeric characters, or a single other character) around `byte`.
fn word_at(text: &str, byte: usize) -> (usize, usize) {
    let byte = floor_boundary(text, byte);
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    let after = text[byte..].chars().next();
    let before = text[..byte].chars().next_back();
    let kind = match (before, after) {
        (_, Some(c)) if is_word(c) => true,
        (Some(c), _) if is_word(c) => true,
        (_, Some(c)) => return (byte, byte + c.len_utf8()),
        (Some(c), None) => return (byte - c.len_utf8(), byte),
        (None, None) => return (byte, byte),
    };
    let start = text[..byte]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_word(c) == kind)
        .last()
        .map_or(byte, |(i, _)| i);
    let end = text[byte..]
        .char_indices()
        .take_while(|&(_, c)| is_word(c) == kind)
        .last()
        .map_or(byte, |(i, c)| byte + i + c.len_utf8());
    (start, end)
}
