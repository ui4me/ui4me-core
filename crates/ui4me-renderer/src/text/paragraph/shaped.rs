//! owner: 009-text-inline (T023; FR-090, FR-091; AD-13; research R-05; 008 `ifc-interface.md`
//! §5.1, §8.7)
//!
//! The shaped-paragraph cache and the layout-epoch registry. `shape` itself lives next to the
//! source types (`source.rs`); this module adds the pure, keyed entry point: identical
//! `ParagraphKey` gives the identical `Arc<ShapedParagraph>` (reuse only when content revision,
//! style epoch and font epoch all match, because all three are part of the key), bounded by an
//! LRU, and the registry of live layout epochs that geometry queries are checked against.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use ui4me_engine_types::codes_generated::Code;
use ui4me_engine_types::stage::Stage;
use ui4me_engine_types::status::Status;

use super::source::{ParagraphKey, ParagraphSource, ShapedParagraph};
use crate::text::fonts::TextServices;

/// Provisional bound (entries) for the shaped-paragraph cache; the G-PERF gate sets the final
/// value (plan Gates).
pub const DEFAULT_SHAPED_CACHE_BOUND: usize = 4096;

/// LRU keyed by `ParagraphKey`.
#[derive(Debug)]
pub struct ShapedCache {
    map: HashMap<ParagraphKey, (Arc<ShapedParagraph>, u64)>,
    tick: u64,
    bound: usize,
}

impl Default for ShapedCache {
    fn default() -> Self {
        ShapedCache {
            map: HashMap::new(),
            tick: 0,
            bound: DEFAULT_SHAPED_CACHE_BOUND,
        }
    }
}

impl ShapedCache {
    fn get(&mut self, key: &ParagraphKey) -> Option<Arc<ShapedParagraph>> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(key).map(|(v, t)| {
            *t = tick;
            v.clone()
        })
    }

    fn insert(&mut self, key: ParagraphKey, value: Arc<ShapedParagraph>) {
        self.tick += 1;
        self.map.insert(key, (value, self.tick));
        self.evict();
    }

    fn evict(&mut self) {
        while self.map.len() > self.bound.max(1) {
            let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| *k)
            else {
                break;
            };
            self.map.remove(&oldest);
        }
    }

    /// Drop every entry of a font epoch other than `epoch`.
    pub(crate) fn retain_font_epoch(&mut self, epoch: u64) {
        self.map.retain(|k, _| k.font_epoch == epoch);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Layout epochs whose frozen paragraphs may still be queried.
#[derive(Debug, Default)]
pub struct LayoutEpochs {
    live: BTreeSet<u64>,
}

impl TextServices {
    /// `shape` cached by `ParagraphKey` (text-services.md §3). The key must describe `source`;
    /// the same key therefore always yields the same shared result.
    pub fn shape_keyed(
        &mut self,
        key: ParagraphKey,
        source: &ParagraphSource,
    ) -> Arc<ShapedParagraph> {
        if let Some(hit) = self.shaped_cache.get(&key) {
            return hit;
        }
        let shaped = self.shape_uncached(source);
        self.shaped_cache.insert(key, shaped.clone());
        shaped
    }

    pub fn shaped_cache_len(&self) -> usize {
        self.shaped_cache.len()
    }

    /// Set the LRU bound (entries, at least 1) and evict down to it.
    pub fn set_shaped_cache_bound(&mut self, bound: usize) {
        self.shaped_cache.bound = bound.max(1);
        self.shaped_cache.evict();
    }

    /// A fragment tree of layout epoch `epoch` now exists; its frozen paragraphs are queryable.
    pub fn begin_layout_epoch(&mut self, epoch: u64) {
        self.epochs.live.insert(epoch);
    }

    /// The last fragment tree of `epoch` was released.
    pub fn release_layout_epoch(&mut self, epoch: u64) {
        self.epochs.live.remove(&epoch);
    }

    /// Geometry queries name the layout epoch of the frozen paragraph; a released (or never
    /// begun) epoch is `SNAPSHOT_RETIRED` (008 `ifc-interface.md` §8.6).
    pub fn check_layout_epoch(&self, epoch: u64) -> Result<(), Status> {
        if self.epochs.live.contains(&epoch) {
            Ok(())
        } else {
            Err(
                Status::new(Code::SnapshotRetired, Stage::Text).with_message(format!(
                    "layout epoch {epoch} was released; the query names a retired snapshot"
                )),
            )
        }
    }
}

/// Hash of the parts of a source that identify it cheaply (equality decides on collisions).
pub(crate) fn content_hash(s: &ParagraphSource) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.text.hash(&mut h);
    s.spans.len().hash(&mut h);
    s.items.len().hash(&mut h);
    s.root_node.hash(&mut h);
    for sp in &s.spans {
        sp.range.start.hash(&mut h);
        sp.owner.hash(&mut h);
    }
    for it in &s.items {
        it.owner.hash(&mut h);
        it.range.start.hash(&mut h);
    }
    for seg in &s.segments {
        seg.processed.start.hash(&mut h);
        if let Some(d) = &seg.dom {
            d.node.hash(&mut h);
            d.start.hash(&mut h);
        }
    }
    h.finish()
}

/// Everything but `dom_revision`.
fn same_content(a: &ParagraphSource, b: &ParagraphSource) -> bool {
    a.document == b.document
        && a.text == b.text
        && a.root == b.root
        && a.spans == b.spans
        && a.items == b.items
        && a.segments == b.segments
        && a.root_node == b.root_node
        && a.base_direction == b.base_direction
        && a.isolated_paragraphs == b.isolated_paragraphs
        && a.fallback_rtl == b.fallback_rtl
        && a.embedding_rtl == b.embedding_rtl
}

/// Passes an unused [`ContentCache`] entry is kept for: a view switched away from and back within a
/// few updates finds its paragraphs. A longer window keeps more garbage from earlier passes (the admin
/// app's start-up passes) and measurably slows the updates until it ages out (with 32, the admin app's
/// first ~30 navigations took ~40 % longer).
pub const RETAIN_PASSES: u64 = 8;
/// Unused [`ContentCache`] entries kept at most.
pub const RETAIN_UNUSED: usize = 4096;

struct ContentEntry {
    font_epoch: u64,
    raw: ParagraphSource,
    shaped: Arc<ShapedParagraph>,
    last_used: u64,
}

/// Cross-pass cache of clean shaping results by paragraph content. Entries used in the previous pass
/// stay; entries not used for [`RETAIN_PASSES`] passes are dropped at the start of the next one
/// (`next_pass`), and at most [`RETAIN_UNUSED`] entries the previous pass did not use are kept (the
/// least recently used go first). So it is bounded by the live document plus a fixed reserve, which
/// keeps the paragraphs of content that was hidden a moment ago (a view switched away from and back,
/// L-PERF-b3) from being shaped again.
#[derive(Default)]
pub struct ContentCache {
    map: HashMap<u64, Vec<ContentEntry>>,
    pass: u64,
    font_epoch: u64,
}

impl std::fmt::Debug for ContentCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentCache")
            .field("entries", &self.len())
            .finish()
    }
}

impl ContentCache {
    pub(crate) fn get(
        &mut self,
        hash: u64,
        source: &ParagraphSource,
        font_epoch: u64,
    ) -> Option<Arc<ShapedParagraph>> {
        let pass = self.pass;
        let e = self
            .map
            .get_mut(&hash)?
            .iter_mut()
            .find(|e| e.font_epoch == font_epoch && same_content(&e.raw, source))?;
        e.last_used = pass;
        Some(e.shaped.clone())
    }

    pub(crate) fn insert(
        &mut self,
        hash: u64,
        source: &ParagraphSource,
        shaped: Arc<ShapedParagraph>,
        font_epoch: u64,
    ) {
        let entry = ContentEntry {
            font_epoch,
            raw: source.clone(),
            shaped,
            last_used: self.pass,
        };
        self.map.entry(hash).or_default().push(entry);
    }

    /// A layout pass begins: age the entries (type docs; everything goes on a font epoch change).
    pub(crate) fn next_pass(&mut self, font_epoch: u64) {
        self.pass += 1;
        if font_epoch != self.font_epoch {
            self.map.clear();
            self.font_epoch = font_epoch;
        }
        let live_from = self.pass - 1;
        let mut keep_from = self.pass.saturating_sub(RETAIN_PASSES);
        // The reserve of unused entries: the most recently used ones, up to the cap.
        let mut unused: Vec<u64> = self
            .map
            .values()
            .flatten()
            .map(|e| e.last_used)
            .filter(|u| *u < live_from && *u >= keep_from)
            .collect();
        if unused.len() > RETAIN_UNUSED {
            let k = unused.len() - RETAIN_UNUSED;
            let (_, nth, _) = unused.select_nth_unstable(k - 1);
            // Everything at or below the k-th oldest goes (ties too: the cap is a bound).
            keep_from = *nth + 1;
        }
        self.map.retain(|_, v| {
            v.retain(|e| e.last_used >= live_from || e.last_used >= keep_from);
            !v.is_empty()
        });
    }

    pub(crate) fn clear(&mut self) {
        self.map.clear();
    }

    pub fn len(&self) -> usize {
        self.map.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
