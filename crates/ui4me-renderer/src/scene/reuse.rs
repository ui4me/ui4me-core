//! owner: L-PERF2 (015 PERF-2) — scene reuse: the paint input of an element box (its `PaintBox`
//! subtree, from which the display-list chunk and the hit regions are built) is taken over from the
//! previous scene when nothing it was projected from changed.
//!
//! **What a `PaintBox` subtree is a function of.** [`crate::scene::appendix_e::prepare_scene`] reads, for one
//! element box and everything below it: (1) the fragments of the subtree (geometry in document space,
//! kinds, the paragraphs their glyph runs come from, the decorations of its lines); (2) for every node
//! it touches (the box owners, text owners, decorating elements, collapsed-border winners, table-track
//! elements) whatever the [`PaintStyleSource`](crate::scene::appendix_e::PaintStyleSource) answers: paint
//! styles, extras, control facts, edit state, column rules and DOM text nodes; (3) the scroll offsets of
//! the scroll containers and controls inside, and sticky shifts (which read every ancestor's offset);
//! (4) its context at entry: the scroll/sticky translation (`shift`), the placeholder colour of an
//! enclosing control, which element's background went to the canvas; (5) whole-tree facts: the box of a
//! table track, the first and last piece of an inline box, every text fragment of a text owner (the DOM
//! text-node cursor walks them in order).
//!
//! An [`Entry`] records (2)-(5) while the subtree is built; it is reused when (1) holds
//! ([`Clean`]: the subtree's fragments equal those of the previous scene's tree, compared field by field,
//! paragraphs by identity), every recorded node is unchanged (`PaintStyleSource::unchanged`: same
//! computed values, no DOM journal entry in its subtree, not focused, reads no image), every recorded
//! scroll offset and track box is the same and the context at entry is equal. Facts of kind (5) are
//! only recorded when they are *contained* in the subtree (else the subtree is not an entry; its reads
//! go to the enclosing entry); a sticky box makes every enclosing subtree volatile. Entries nest: an
//! entry lists the entries directly inside it, and its own reads cover only the rest. The scene-stage
//! side effects of a build (reports, consumption checks, font blobs) are recorded in order and replayed.
//! Every entry the next scene may use was either built or validated by this scene, so "unchanged since
//! the previous scene" chains back to the scene it was built in.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use crate::layout::box_tree::BoxId;
use crate::layout::coordinator::LayoutOutput;
use crate::layout::fragment::{FragmentId, FragmentKind, FragmentTree};
use ui4me_engine_types::geometry::LayoutPoint;
use ui4me_engine_types::ids::NodeId;

use crate::scene::appendix_e::PaintBox;
use crate::scene::display_list::Color;

/// An element box (its id in the box tree; entries are valid only for the same box id anyway).
pub(crate) type Key = BoxId;

/// A fast, non-keyed hasher for the reuse maps (node ids and small keys; FxHash's multiply-rotate):
/// SipHash was a fifth of the validation time (015 PERF-2). Not DoS-resistant; the keys are ours.
#[derive(Clone, Copy, Default)]
pub struct FastHasher(u64);

impl std::hash::Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut w = [0u8; 8];
            w[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(w));
        }
    }
    fn write_u8(&mut self, i: u8) {
        self.write_u64(u64::from(i));
    }
    fn write_u16(&mut self, i: u16) {
        self.write_u64(u64::from(i));
    }
    fn write_u32(&mut self, i: u32) {
        self.write_u64(u64::from(i));
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

/// Values by box id (box ids are dense pre-order indices): no hashing on the reuse path.
pub(crate) struct ByBox<T>(Vec<Option<T>>);

impl<T> Default for ByBox<T> {
    fn default() -> Self {
        ByBox(Vec::new())
    }
}

impl<T> ByBox<T> {
    pub fn with_len(n: usize) -> ByBox<T> {
        ByBox::default().reset(n)
    }

    /// Empty, `n` slots, the storage kept.
    pub fn reset(mut self, n: usize) -> ByBox<T> {
        self.0.clear();
        self.0.resize_with(n, || None);
        self
    }

    pub fn get(&self, k: &Key) -> Option<&T> {
        self.0.get(k.0 as usize)?.as_ref()
    }

    pub fn take(&mut self, k: &Key) -> Option<T> {
        self.0.get_mut(k.0 as usize)?.take()
    }

    pub fn insert(&mut self, k: Key, v: T) {
        let i = k.0 as usize;
        if i >= self.0.len() {
            self.0.resize_with(i + 1, || None);
        }
        self.0[i] = Some(v);
    }

    pub fn len(&self) -> usize {
        self.0.iter().filter(|e| e.is_some()).count()
    }
}

/// A `HashMap` with [`FastHasher`].
pub type FastMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<FastHasher>>;
/// A `HashSet` with [`FastHasher`].
pub type FastSet<K> = std::collections::HashSet<K, std::hash::BuildHasherDefault<FastHasher>>;

/// Scene reuse state of one document, kept by the caller from one scene to the next (015 PERF-2).
/// Empty = nothing to reuse; [`SceneMemo::clear`] forgets everything.
#[derive(Default)]
pub struct SceneMemo {
    /// The layout the last scene was built from (alive while it is the committed one).
    pub(crate) built_from: Weak<LayoutOutput>,
    pub(crate) entries: ByBox<Arc<Entry>>,
    /// The previous scene's table, emptied: the next scene's table reuses its storage.
    pub(crate) spare: ByBox<Arc<Entry>>,
    /// Per-scene scratch tables kept for their storage.
    pub(crate) spare_ok: ByBox<bool>,
    /// `PaintOptions::paint_radii` of the last scene.
    pub(crate) paint_radii: Option<bool>,
    pub(crate) stats: ReuseStats,
}

impl std::fmt::Debug for SceneMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneMemo")
            .field("entries", &self.entries.len())
            .field("stats", &self.stats)
            .finish()
    }
}

/// What the last scene built with a [`SceneMemo`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReuseStats {
    /// Element-box subtrees taken over (outermost only).
    pub reused: u32,
    /// Element-box subtrees projected and recorded.
    pub built: u32,
}

impl SceneMemo {
    pub fn new() -> SceneMemo {
        SceneMemo::default()
    }

    /// Forget every entry (the next scene builds everything).
    pub fn clear(&mut self) {
        *self = SceneMemo::default();
    }

    pub fn stats(&self) -> ReuseStats {
        self.stats
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }
}

/// A side effect of building a subtree, replayed when it is reused.
pub(crate) enum Event {
    /// An entry directly inside.
    Nested(Key),
    Report(&'static str, NodeId),
    Checked(NodeId),
    Font(u64, Arc<[u8]>),
}

/// One reusable element-box subtree.
pub(crate) struct Entry {
    pub box_id: BoxId,
    pub paint: Arc<PaintBox>,
    pub shift: LayoutPoint,
    pub placeholder: Option<Color>,
    pub propagated: Option<NodeId>,
    /// Nodes read outside the nested entries.
    pub reads: Box<[NodeId]>,
    pub scroll: Box<[(NodeId, (f64, f64))]>,
    pub boxes: Box<[(BoxId, Option<NodeId>)]>,
    pub events: Box<[Event]>,
}

/// An element-box subtree being built.
pub(crate) struct Frame {
    pub key: Key,
    pub box_id: BoxId,
    pub root: FragmentId,
    pub shift: LayoutPoint,
    pub placeholder: Option<Color>,
    pub propagated: Option<NodeId>,
    pub reads: Vec<NodeId>,
    pub scroll: Vec<(NodeId, (f64, f64))>,
    pub boxes: Vec<(BoxId, Option<NodeId>)>,
    pub events: Vec<Event>,
    pub fonts: Vec<u64>,
    /// Fragments that must lie in the subtree (first and last pieces of inline boxes).
    pub contain: Vec<FragmentId>,
    /// Elements whose every text fragment must lie in the subtree.
    pub text_owners: Vec<NodeId>,
    pub volatile: bool,
}

impl Frame {
    pub fn new(
        key: Key,
        box_id: BoxId,
        root: FragmentId,
        shift: LayoutPoint,
        placeholder: Option<Color>,
        propagated: Option<NodeId>,
    ) -> Frame {
        Frame {
            key,
            box_id,
            root,
            shift,
            placeholder,
            propagated,
            reads: Vec::new(),
            scroll: Vec::new(),
            boxes: Vec::new(),
            events: Vec::new(),
            fonts: Vec::new(),
            contain: Vec::new(),
            text_owners: Vec::new(),
            volatile: false,
        }
    }

    /// A subtree inside that cannot be an entry: its reads become this one's.
    pub fn absorb(&mut self, f: Frame) {
        self.reads.extend(f.reads);
        self.scroll.extend(f.scroll);
        self.boxes.extend(f.boxes);
        self.events.extend(f.events);
        for id in f.fonts {
            if !self.fonts.contains(&id) {
                self.fonts.push(id);
            }
        }
        self.contain.extend(f.contain);
        self.text_owners.extend(f.text_owners);
        self.volatile |= f.volatile;
    }

    pub fn into_entry(self, paint: Arc<PaintBox>) -> Entry {
        let mut reads = self.reads;
        reads.sort_unstable();
        reads.dedup();
        Entry {
            box_id: self.box_id,
            paint,
            shift: self.shift,
            placeholder: self.placeholder,
            propagated: self.propagated,
            reads: reads.into_boxed_slice(),
            scroll: self.scroll.into_boxed_slice(),
            boxes: self.boxes.into_boxed_slice(),
            events: self.events.into_boxed_slice(),
        }
    }
}

/// The reuse state of one scene build.
pub(crate) struct Reuse<'a> {
    /// `None`: no previous tree to compare with (entries are only recorded).
    pub clean: Option<Clean<'a>>,
    pub prev: ByBox<Arc<Entry>>,
    pub next: ByBox<Arc<Entry>>,
    pub inputs_ok: ByBox<bool>,
    /// Per box id of the tree being painted, its only box fragment (`single_fragment_of_box`).
    pub new_of_box: Vec<u32>,
    pub stack: Vec<Frame>,
    pub stats: ReuseStats,
}

/// Fragment-subtree equality between the tree being painted and the previous scene's tree, memoised per
/// new fragment.
pub(crate) struct Clean<'a> {
    new: &'a FragmentTree,
    old: &'a FragmentTree,
    /// Both scenes paint the same tree (a reused layout).
    same_tree: bool,
    /// Per new fragment: 0 not compared, 1 differs, 2 equal.
    memo: Vec<u8>,
    /// Per box id, its only box fragment in the old tree (`NONE`: none or several).
    old_of_box: Vec<u32>,
    /// Per paragraph index: 0 not compared, 1 differs, 2 equal.
    para_memo: Vec<u8>,
}

/// Equal glyph runs (everything the scene reads of them).
fn same_runs(
    a: &[crate::layout::fragment::GlyphRunView],
    b: &[crate::layout::fragment::GlyphRunView],
) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.font.blob.id.0 == y.font.blob.id.0
                && x.font.index == y.font.index
                && x.font.coords == y.font.coords
                && x.font.embolden == y.font.embolden
                && x.font.oblique_angle_deg == y.font.oblique_angle_deg
                && x.size_px == y.size_px
                && x.origin == y.origin
                && x.span == y.span
                && x.owner == y.owner
                && x.bidi_level == y.bidi_level
                && x.glyphs.len() == y.glyphs.len()
                && x.glyphs.iter().zip(&y.glyphs).all(|(g, h)| {
                    g.id == h.id && g.x == h.x && g.y == h.y && g.advance == h.advance
                })
        })
}

const NONE: u32 = u32::MAX;

/// Per box id, its only box fragment in `tree` (`NONE` when it has none or several); one pass instead of a
/// hashed lookup per box.
pub(crate) fn single_fragment_of_box(tree: &FragmentTree) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for f in &tree.fragments {
        if let FragmentKind::Box(b) = &f.kind {
            let i = b.box_id.0 as usize;
            if i >= out.len() {
                out.resize(i + 1, NONE - 1);
            }
            out[i] = if out[i] == NONE - 1 { f.id.0 } else { NONE };
        }
    }
    for v in &mut out {
        if *v == NONE - 1 {
            *v = NONE;
        }
    }
    out
}

impl<'a> Clean<'a> {
    pub fn new(new: &'a FragmentTree, old: &'a FragmentTree, same_tree: bool) -> Clean<'a> {
        let (memo, old_of_box) = if same_tree {
            (Vec::new(), Vec::new())
        } else {
            (vec![0; new.fragments.len()], single_fragment_of_box(old))
        };
        Clean {
            new,
            old,
            same_tree,
            memo,
            old_of_box,
            para_memo: Vec::new(),
        }
    }

    /// Whether box fragment `n` (its box's only fragment) and its subtree equal the previous tree's only
    /// fragment of the same box.
    pub fn box_eq(&mut self, n: FragmentId) -> bool {
        if self.same_tree {
            return true;
        }
        let FragmentKind::Box(b) = &self.new.get(n).kind else {
            return false;
        };
        match self.old_of_box.get(b.box_id.0 as usize) {
            Some(&o) if o != NONE => self.eq(n, FragmentId(o)),
            _ => false,
        }
    }

    fn eq(&mut self, n: FragmentId, o: FragmentId) -> bool {
        let i = n.0 as usize;
        match self.memo[i] {
            1 => return false,
            2 => return true,
            _ => {}
        }
        let r = self.eq_uncached(n, o);
        self.memo[i] = if r { 2 } else { 1 };
        r
    }

    /// Paragraph `i` of both trees is the same: the same object, or (a paragraph laid out again, as every
    /// block container's is when it is not a replay root) the same text, lines and glyph runs, which is
    /// all the scene reads of it. Memoised per paragraph.
    fn para(&mut self, i: Option<usize>) -> bool {
        let Some(i) = i else { return true };
        let (a, b) = match (self.new.paragraphs.get(i), self.old.paragraphs.get(i)) {
            (Some(a), Some(b)) => (a, b),
            (None, None) => return true,
            _ => return false,
        };
        if Arc::ptr_eq(a, b) {
            return true;
        }
        if self.para_memo.len() <= i {
            self.para_memo.resize(i + 1, 0);
        }
        match self.para_memo[i] {
            1 => return false,
            2 => return true,
            _ => {}
        }
        let same = a.text() == b.text()
            && a.line_count() == b.line_count()
            && (0..a.line_count())
                .all(|l| a.with_glyph_runs(l, |ra| b.with_glyph_runs(l, |rb| same_runs(ra, rb))));
        self.para_memo[i] = if same { 2 } else { 1 };
        same
    }

    fn eq_uncached(&mut self, n: FragmentId, o: FragmentId) -> bool {
        let (new, old) = (self.new, self.old);
        let (a, b) = (new.get(n), old.get(o));
        if a.origin != b.origin
            || a.size != b.size
            || a.children.len() != b.children.len()
            || a.kind != b.kind
        {
            return false;
        }
        if new.document_rect(n).rect != old.document_rect(o).rect {
            return false;
        }
        // Equal kinds have equal paragraph indices; the paragraphs behind them must be the same objects.
        let paragraphs_ok = match &a.kind {
            FragmentKind::Box(bx) => {
                self.para(bx.paragraph_index)
                    && self.para(
                        bx.extras
                            .control_inner
                            .as_ref()
                            .and_then(|c| c.paragraph_index),
                    )
            }
            FragmentKind::Line(l) => {
                let (da, db) = (new.decorations_of(n), old.decorations_of(o));
                self.para(Some(l.paragraph_index))
                    && da.len() == db.len()
                    && da.iter().zip(db).all(|(x, y)| {
                        x.origin == y.origin
                            && x.size == y.size
                            && x.owner == y.owner
                            && x.kind == y.kind
                            && x.style == y.style
                            && x.owner_pseudo == y.owner_pseudo
                            && x.thickness == y.thickness
                    })
            }
            FragmentKind::Text(t) => self.para(Some(t.paragraph_index)),
            FragmentKind::Marker(m) => self.para(m.paragraph_index),
            FragmentKind::InlineBoxPiece(_)
            | FragmentKind::TableTrack(_)
            | FragmentKind::CollapsedBorderSegment(_)
            | FragmentKind::Fragmentainer(_) => true,
        };
        if !paragraphs_ok {
            return false;
        }
        a.children
            .iter()
            .zip(&b.children)
            .all(|(cn, co)| self.eq(*cn, *co))
    }
}

/// Whether fragment `f` is `root` or one of its descendants in `tree`.
pub(crate) fn descends(tree: &FragmentTree, mut f: FragmentId, root: FragmentId) -> bool {
    loop {
        if f == root {
            return true;
        }
        match tree.get(f).parent {
            Some(p) => f = p,
            None => return false,
        }
    }
}
